//! Changes for writers with no steps. [`diff`] builds the change that takes
//! one value to another over a word-aligned text diff; [`align`] gives a value
//! read fresh from markdown the ids of what it kept; [`revise`] and
//! [`replace`] are the producers a markdown rewrite and a whole-value write
//! use.

use crate::change::{
    AnchorStep, Change, ContainerStep, IslandStep, SetBlock, Splice, Step, fixups, run,
};
use crate::ids::{Minter, prefix};
use crate::lines::delta::{Op, diff_import, diff_words as text_diff, without_sentinel};
use crate::lines::import::{ImportError, ImportWarning};
use crate::migrate::{from_lines, to_lines};
use crate::model::{Content, ISLAND_SLOT, Island, Mark};
use crate::table::{self, Table};
use std::collections::{HashMap, HashSet};

/// The change that takes `before` to `after`. What no change can reach comes
/// out under `before`'s ids instead: the last block's record, which no `\n`
/// carries, and a container whose id `before` holds with another kind.
pub fn diff(before: &Content, after: &Content) -> Change {
    Change::new(diff_level(before, after, false, true))
}

/// [`diff`], leaving every anchor of `before` that `after` does not name to
/// ride the text it covers.
pub fn diff_carrying(before: &Content, after: &Content) -> Change {
    Change::new(diff_level(before, after, true, true))
}

fn diff_level(before: &Content, after: &Content, carry: bool, top: bool) -> Vec<Step> {
    let mut want = reachable(before, after, top);
    let moved = moved_ids(&splices(before, &want));
    if !moved.is_empty() {
        let mut ids = Minter::default();
        for id in before.blocks.iter().chain(&want.blocks).map(|b| &b.id) {
            ids.hold(id);
        }
        for id in before.islands.iter().chain(&want.islands).map(|i| &i.id) {
            ids.hold(id);
        }
        for b in &mut want.blocks {
            if moved.contains(&b.id) {
                b.id = ids.mint(prefix::BLOCK);
            }
        }
        for i in &mut want.islands {
            if moved.contains(&i.id) {
                i.id = ids.mint(prefix::ISLAND);
            }
        }
    }
    let mut steps = Vec::new();
    for k in &want.containers {
        if before.container(&k.id) != Some(k) {
            steps.push(Step::Container(ContainerStep::Set(k.clone())));
        }
    }
    steps.extend(splices(before, &want).into_iter().map(|mut s| {
        if let Step::Splice(sp) = &mut s {
            sp.removed = None;
        }
        s
    }));
    let mut cur = before.clone();
    if run(&mut cur, &steps, &mut Vec::new(), &mut Vec::new()).is_err() {
        return replace_all(before, &want, carry);
    }
    let mut rest = Vec::new();
    for (have, need) in cur.blocks.iter().zip(&want.blocks) {
        if have.kind != need.kind || have.path != need.path {
            rest.push(Step::SetBlock(SetBlock {
                id: have.id.clone(),
                kind: Some(need.kind.tag().to_string()),
                attrs: crate::wire::block_attrs(&need.kind).and_then(|v| v.as_object().cloned()),
                path: Some(need.path.clone()),
            }));
        }
    }
    for (have, need) in cur.islands.iter().zip(&want.islands) {
        if have.props != need.props {
            let slots = need.slots.iter().filter(|(k, _)| !have.slots.contains_key(*k)).map(|(k, v)| (k.clone(), v.clone())).collect();
            rest.push(Step::Island(IslandStep::Props { id: have.id.clone(), props: need.props.clone(), slots }));
        }
        for (slot, inner) in &need.slots {
            if let Some(held) = have.slots.get(slot)
                && held != inner
            {
                let change = Change::new(diff_level(held, inner, carry, false));
                if !change.is_empty() {
                    rest.push(Step::Nested { island: have.id.clone(), slot: slot.clone(), change });
                }
            }
        }
    }
    if run(&mut cur, &rest, &mut Vec::new(), &mut Vec::new()).is_err() {
        return replace_all(before, &want, carry);
    }
    steps.extend(rest);
    steps.extend(fixups(&cur, &want).into_iter().filter(|s| !(carry && matches!(s, Step::Anchor(AnchorStep::Remove { .. })))));
    steps
}

/// The block and island ids one splice inserts and a later one deletes: a
/// record that moves ahead, which would be held twice between the two, so it
/// moves under a fresh id.
fn moved_ids(steps: &[Step]) -> HashSet<String> {
    let mut inserted: HashMap<String, usize> = HashMap::new();
    let mut deleted: HashMap<String, usize> = HashMap::new();
    for (n, step) in steps.iter().enumerate() {
        let Step::Splice(s) = step else { continue };
        for id in s.blocks.iter().map(|b| &b.id).chain(s.islands.iter().map(|i| &i.id)) {
            inserted.insert(id.clone(), n);
        }
        if let Some(r) = &s.removed {
            for id in r.blocks.iter().map(|b| &b.id).chain(r.islands.iter().map(|i| &i.id)) {
                deleted.insert(id.clone(), n);
            }
        }
    }
    inserted.into_iter().filter(|(id, n)| deleted.get(id).is_some_and(|m| m > n)).map(|(id, _)| id).collect()
}

/// One splice over the whole text, for a pair the step-wise diff cannot
/// bridge.
fn replace_all(before: &Content, want: &Content, carry: bool) -> Vec<Step> {
    let mut steps: Vec<Step> = want
        .containers
        .iter()
        .filter(|k| before.container(&k.id) != Some(*k))
        .map(|k| Step::Container(ContainerStep::Set(k.clone())))
        .collect();
    let n = want.blocks.len();
    let splice = Splice {
        marks: want.marks.clone(),
        blocks: want.blocks[..n - 1].to_vec(),
        islands: want.islands.clone(),
        ..Splice::new(0, before.text.clone(), want.text.clone())
    };
    steps.push(Step::Splice(splice));
    let last = &want.blocks[n - 1];
    steps.push(Step::SetBlock(SetBlock {
        id: last.id.clone(),
        kind: Some(last.kind.tag().to_string()),
        attrs: crate::wire::block_attrs(&last.kind).and_then(|v| v.as_object().cloned()),
        path: Some(last.path.clone()),
    }));
    let mut cur = before.clone();
    if run(&mut cur, &steps, &mut Vec::new(), &mut Vec::new()).is_ok() {
        steps.extend(fixups(&cur, want).into_iter().filter(|s| !(carry && matches!(s, Step::Anchor(AnchorStep::Remove { .. })))));
    }
    steps
}

/// `after` under the ids a change from `before` can give it.
fn reachable(before: &Content, after: &Content, top: bool) -> Content {
    let mut want = after.clone();
    let mut ids = Minter::default();
    for b in before.blocks.iter().chain(&want.blocks) {
        ids.hold(&b.id);
    }
    let last = before.blocks.last().map(|b| b.id.clone()).unwrap_or_default();
    let n = want.blocks.len();
    for b in &mut want.blocks[..n - 1] {
        if b.id == last {
            b.id = ids.mint(prefix::BLOCK);
        }
    }
    want.blocks[n - 1].id = last;
    if top {
        let mut renamed = HashMap::new();
        for k in &mut want.containers {
            if before.container(&k.id).is_some_and(|held| !held.kind.same_kind(&k.kind)) {
                let fresh = fresh_id(&k.id, before, after);
                renamed.insert(k.id.clone(), fresh.clone());
                k.id = fresh;
            }
        }
        if !renamed.is_empty() {
            for b in &mut want.blocks {
                for id in &mut b.path {
                    if let Some(to) = renamed.get(id) {
                        *id = to.clone();
                    }
                }
            }
        }
    }
    want
}

fn fresh_id(id: &str, before: &Content, after: &Content) -> String {
    let taken: HashSet<&str> = before.containers.iter().chain(&after.containers).map(|k| k.id.as_str()).collect();
    (1..).map(|n| format!("{id}-{n}")).find(|c| !taken.contains(c.as_str())).expect("a free id")
}

/// The splices of a word-aligned diff of the two texts, a kept `\n` or slot
/// whose record differs deleted and inserted again.
fn splices(before: &Content, want: &Content) -> Vec<Step> {
    let a: Vec<char> = before.text.chars().collect();
    let b: Vec<char> = want.text.chars().collect();
    let records = |chars: &[char]| -> Vec<Option<usize>> {
        let (mut nl, mut sl) = (0, 0);
        chars
            .iter()
            .map(|&ch| match ch {
                '\n' => {
                    nl += 1;
                    Some(nl - 1)
                }
                ISLAND_SLOT => {
                    sl += 1;
                    Some(sl - 1)
                }
                _ => None,
            })
            .collect()
    };
    let (ra, rb) = (records(&a), records(&b));
    let same = |i: usize, j: usize| match (a[i], ra[i], rb[j]) {
        ('\n', Some(x), Some(y)) => before.blocks[x].id == want.blocks[y].id,
        (ISLAND_SLOT, Some(x), Some(y)) => {
            let (p, q) = (&before.islands[x], &want.islands[y]);
            p.id == q.id && p.island_type == q.island_type
        }
        _ => true,
    };
    let mut steps = Vec::new();
    let (mut i, mut j, mut pos) = (0usize, 0usize, 0usize);
    let (mut i0, mut j0) = (0usize, 0usize);
    let flush = |i: usize, j: usize, i0: &mut usize, j0: &mut usize, pos: &mut usize, steps: &mut Vec<Step>| {
        if i > *i0 || j > *j0 {
            let mut s = splice_of(want, &a[*i0..i], &b[*j0..j], *j0, &rb[*j0..j], *pos);
            s.removed = Some(crate::change::Removed {
                blocks: (*i0..i).filter(|&k| a[k] == '\n').filter_map(|k| ra[k]).map(|x| before.blocks[x].clone()).collect(),
                islands: (*i0..i).filter(|&k| a[k] == ISLAND_SLOT).filter_map(|k| ra[k]).map(|x| before.islands[x].clone()).collect(),
                marks: Vec::new(),
            });
            steps.push(Step::Splice(s));
            *pos += j - *j0;
        }
        *i0 = i;
        *j0 = j;
    };
    for op in text_ops(&before.text, &want.text) {
        match op {
            Op::Retain(n) => {
                for _ in 0..n {
                    if same(i, j) {
                        flush(i, j, &mut i0, &mut j0, &mut pos, &mut steps);
                        i += 1;
                        j += 1;
                        i0 = i;
                        j0 = j;
                        pos += 1;
                    } else {
                        i += 1;
                        j += 1;
                    }
                }
            }
            Op::Delete(n) => i += n,
            Op::Insert(s) => j += s.chars().count(),
        }
    }
    flush(i, j, &mut i0, &mut j0, &mut pos, &mut steps);
    steps
}

fn splice_of(want: &Content, del: &[char], ins: &[char], from: usize, records: &[Option<usize>], at: usize) -> Splice {
    let mut blocks = Vec::new();
    let mut islands = Vec::new();
    for (&ch, r) in ins.iter().zip(records) {
        match (ch, r) {
            ('\n', Some(k)) => blocks.push(want.blocks[*k].clone()),
            (ISLAND_SLOT, Some(k)) => islands.push(want.islands[*k].clone()),
            _ => {}
        }
    }
    let to = from + ins.len();
    let marks = want
        .marks
        .iter()
        .filter_map(|m| {
            let (lo, hi) = (m.start.max(from), m.end.min(to));
            (lo < hi).then(|| Mark::new(lo - from, hi - from, m.kind.clone()))
        })
        .collect();
    Splice {
        marks,
        blocks,
        islands,
        ..Splice::new(at, del.iter().collect::<String>(), ins.iter().collect::<String>())
    }
}

/// The word-aligned text diff, a last block compared whole with its newline as
/// every other block is, so text appended after it keeps it.
fn text_ops(a: &str, b: &str) -> Vec<Op> {
    if !a.contains('\n') && !b.contains('\n') {
        return text_diff(a, b).ops;
    }
    let b = format!("{b}\n");
    without_sentinel(text_diff(&format!("{a}\n"), &b).ops, &b)
}

/// `after` under the ids of what it kept from `before`: each block whose
/// `\n` the text diff keeps, the last block, each island whose slot it keeps
/// with its type or that an island it drops further back held under the same
/// id and type, a kept table's columns, rows and cells by position, and each
/// container a kept block's path names at the same depth with the same kind.
/// Every other id is kept where `before` does not hold it, and minted
/// otherwise.
pub fn align(before: &Content, after: Content) -> Content {
    let mut after = after;
    let a: Vec<char> = before.text.chars().collect();
    let b: Vec<char> = after.text.chars().collect();
    let mut blocks: Vec<(usize, usize)> = Vec::new();
    let mut islands: Vec<(usize, usize)> = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    let (mut nl_a, mut nl_b, mut sl_a, mut sl_b) = (0usize, 0usize, 0usize, 0usize);
    let (mut dropped_at, mut added_at): (HashMap<usize, usize>, HashMap<usize, usize>) = (HashMap::new(), HashMap::new());
    let mut event = 0usize;
    let mut step = |ia: Option<usize>, jb: Option<usize>, blocks: &mut Vec<(usize, usize)>, islands: &mut Vec<(usize, usize)>| {
        let ca = ia.map(|k| a[k]);
        let cb = jb.map(|k| b[k]);
        event += 1;
        if ca == Some('\n') && cb == Some('\n') {
            blocks.push((nl_a, nl_b));
        }
        if ca == Some(ISLAND_SLOT) && cb == Some(ISLAND_SLOT) {
            islands.push((sl_a, sl_b));
        } else if ca == Some(ISLAND_SLOT) {
            dropped_at.insert(sl_a, event);
        } else if cb == Some(ISLAND_SLOT) {
            added_at.insert(sl_b, event);
        }
        if ca == Some('\n') {
            nl_a += 1;
        }
        if cb == Some('\n') {
            nl_b += 1;
        }
        if ca == Some(ISLAND_SLOT) {
            sl_a += 1;
        }
        if cb == Some(ISLAND_SLOT) {
            sl_b += 1;
        }
    };
    for op in text_ops(&before.text, &after.text) {
        match op {
            Op::Retain(n) => {
                for _ in 0..n {
                    step(Some(i), Some(j), &mut blocks, &mut islands);
                    i += 1;
                    j += 1;
                }
            }
            Op::Delete(n) => {
                for _ in 0..n {
                    step(Some(i), None, &mut blocks, &mut islands);
                    i += 1;
                }
            }
            Op::Insert(s) => {
                for _ in s.chars() {
                    step(None, Some(j), &mut blocks, &mut islands);
                    j += 1;
                }
            }
        }
    }
    blocks.push((before.blocks.len() - 1, after.blocks.len() - 1));

    let mut block_to: HashMap<usize, String> = HashMap::new();
    let mut containers: HashMap<String, String> = HashMap::new();
    let mut taken: HashSet<String> = HashSet::new();
    for &(x, y) in &blocks {
        block_to.insert(y, before.blocks[x].id.clone());
        let (pa, pb) = (&before.blocks[x].path, &after.blocks[y].path);
        for (ka, kb) in pa.iter().zip(pb) {
            let same_kind = match (before.container(ka), after.container(kb)) {
                (Some(p), Some(q)) => p.kind.same_kind(&q.kind),
                _ => false,
            };
            if !same_kind {
                break;
            }
            match containers.get(kb) {
                Some(to) if to == ka => continue,
                Some(_) => break,
                None if taken.contains(ka) => break,
                None => {
                    containers.insert(kb.clone(), ka.clone());
                    taken.insert(ka.clone());
                }
            }
        }
    }
    let mut ids = Minter::default();
    for id in before.blocks.iter().map(|b| &b.id).chain(before.containers.iter().map(|k| &k.id)) {
        ids.hold(id);
    }
    for id in block_to.values().chain(containers.values()) {
        ids.hold(id);
    }
    for k in &after.containers {
        if !containers.contains_key(&k.id) {
            let kept = if before.container(&k.id).is_none() && !taken.contains(&k.id) {
                ids.hold(&k.id);
                k.id.clone()
            } else {
                ids.mint(container_prefix(k))
            };
            taken.insert(kept.clone());
            containers.insert(k.id.clone(), kept);
        }
    }
    let held: HashSet<&str> = before.blocks.iter().map(|b| b.id.as_str()).collect();
    for (y, b) in after.blocks.iter_mut().enumerate() {
        b.id = match block_to.get(&y) {
            Some(id) => id.clone(),
            None if !held.contains(b.id.as_str()) && !ids.holds(&b.id) => {
                ids.hold(&b.id);
                b.id.clone()
            }
            None => ids.mint(prefix::BLOCK),
        };
        for id in &mut b.path {
            if let Some(to) = containers.get(id) {
                *id = to.clone();
            }
        }
    }
    for k in &mut after.containers {
        if let Some(to) = containers.get(&k.id) {
            k.id = to.clone();
        }
    }

    let mut island_ids = Minter::default();
    for i in &before.islands {
        island_ids.hold(&i.id);
    }
    let matched: HashMap<usize, usize> = islands
        .into_iter()
        .filter(|&(x, y)| before.islands[x].island_type == after.islands[y].island_type)
        .map(|(x, y)| (y, x))
        .collect();
    let mut unclaimed: HashMap<&str, (usize, &Island)> = before
        .islands
        .iter()
        .enumerate()
        .filter_map(|(x, i)| Some((i.id.as_str(), (*dropped_at.get(&x)?, i))))
        .collect();
    for (y, island) in after.islands.iter_mut().enumerate() {
        match matched.get(&y) {
            Some(&x) => align_island(&before.islands[x], island),
            None => {
                let moved = unclaimed.get(island.id.as_str()).copied().filter(|(at, held)| {
                    held.island_type == island.island_type && added_at.get(&y).is_some_and(|added| at < added)
                });
                if let Some((_, held)) = moved {
                    unclaimed.remove(held.id.as_str());
                    align_island(held, island);
                } else if island_ids.holds(&island.id) {
                    island.id = island_ids.mint(prefix::ISLAND);
                } else {
                    island_ids.hold(&island.id);
                }
            }
        }
    }
    after.normalize();
    after
}

fn container_prefix(k: &crate::model::Container) -> &'static str {
    match k.kind {
        crate::model::ContainerKind::List { .. } => prefix::LIST,
        crate::model::ContainerKind::Item { .. } => prefix::ITEM,
        crate::model::ContainerKind::Quote => prefix::QUOTE,
    }
}

/// `island` under `held`'s id, a table's grid under `held`'s ids by
/// position, and each slot's content aligned to the slot it now names.
fn align_island(held: &Island, island: &mut Island) {
    island.id = held.id.clone();
    if let (Some(old), Some(new)) = (Table::read(&held.props), Table::read(&island.props)) {
        let mut ids = Minter::default();
        for id in old.columns.iter().chain(old.rows.iter().map(|r| &r.id)).chain(old.slot_order().iter()) {
            ids.hold(id);
        }
        let by_position = |a: &[String], b: &[String], ids: &mut Minter, p: &str| -> HashMap<String, String> {
            b.iter()
                .enumerate()
                .map(|(n, id)| {
                    let to = a.get(n).cloned().unwrap_or_else(|| ids.mint(p));
                    (id.clone(), to)
                })
                .collect()
        };
        let columns = by_position(&old.columns, &new.columns, &mut ids, prefix::COLUMN);
        let rows = by_position(
            &old.rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
            &new.rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
            &mut ids,
            prefix::ROW,
        );
        let mut slots = HashMap::new();
        for (r, row) in new.rows.iter().enumerate() {
            for (c, slot) in row.cells.iter().enumerate() {
                let to = old.slot(r, c).map(str::to_string).unwrap_or_else(|| ids.mint(prefix::SLOT));
                slots.insert(slot.clone(), to);
            }
        }
        table::rename(island, &columns, &rows, &slots);
    }
    for (id, slot) in island.slots.iter_mut() {
        if let Some(old) = held.slots.get(id) {
            *slot = align(old, std::mem::take(slot));
        }
    }
}

/// A markdown rewrite of `before`, as v1's revise reads it: anchors kept by
/// the tags the markdown kept, otherwise by the word-aligned diff.
#[derive(Debug, Clone, PartialEq)]
pub struct Revision {
    pub change: Change,
    pub content: Content,
    pub warnings: Vec<ImportWarning>,
}

pub fn revise(before: &Content, markdown: &str) -> Result<Revision, ImportError> {
    let (lines, _, warnings) = diff_import(&to_lines(before), markdown)?;
    let content = align(before, from_lines(&lines));
    let change = diff(before, &content);
    Ok(Revision { change, content, warnings })
}

/// The change that writes `content` over `before`: each anchor of `before`
/// that `content` does not name carried where the diff keeps its text.
pub fn replace(before: &Content, content: &Content) -> Change {
    diff_carrying(before, content)
}

/// A line-form value written over `before`, under the ids of what it kept.
pub fn lift(before: &Content, after: &crate::lines::model::Content) -> Content {
    align(before, from_lines(after))
}
