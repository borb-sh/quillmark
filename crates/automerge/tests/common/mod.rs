//! The generators `crates/content/tests/change_laws.rs` uses: values with
//! headings, lists, quotes, code, tables, footnotes, math, `x-` blocks, marks
//! and islands, anchors in text and in slots, and changes of every step kind.

#![allow(dead_code)]

use proptest::prelude::*;
use quillmark_content::change::{AnchorStep, Change, ContainerStep, IslandStep, MarkStep, SetBlock, Splice, Step, apply};
use quillmark_content::lines::island::IslandType;
use quillmark_content::markdown::from_markdown;
use quillmark_content::model::{Anchor, Block, BlockKind, Container, ContainerKind, Content, ISLAND_SLOT, Island, Mark, MarkKind};
use quillmark_content::table::{Row, Table};
use serde_json::json;

/// A case count, `var` where it is set.
pub fn cases(var: &str, default: u32) -> u32 {
    std::env::var(var).ok().and_then(|n| n.parse().ok()).unwrap_or(default)
}

pub fn word() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-z]{1,5}",
        "[a-z]{1,4}".prop_map(|w| format!("**{w}**")),
        "[a-z]{1,4}".prop_map(|w| format!("_{w}_")),
        "[a-z]{1,4}".prop_map(|w| format!("[{w}](https://x.io/{w})")),
        Just("$x^2$".to_string()),
        Just("[^n]".to_string()),
        Just("é😀".to_string()),
    ]
}

pub fn line() -> impl Strategy<Value = String> {
    prop::collection::vec(word(), 1..5).prop_map(|w| w.join(" "))
}

pub fn block() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => line(),
        1 => line().prop_map(|l| format!("## {l}")),
        1 => prop::collection::vec(line(), 1..4).prop_map(|ls| ls.iter().map(|l| format!("- {l}")).collect::<Vec<_>>().join("\n")),
        1 => prop::collection::vec(line(), 1..3).prop_map(|ls| ls.iter().map(|l| format!("1. {l}")).collect::<Vec<_>>().join("\n")),
        1 => (line(), line()).prop_map(|(a, b)| format!("> {a}\n>\n> {b}")),
        1 => (line(), line()).prop_map(|(a, b)| format!("{a}\\\n{b}")),
        1 => ("[a-z]{1,4}", "[a-z]{1,4}", "[a-z]{1,4}", "[a-z]{1,4}")
            .prop_map(|(a, b, c, d)| format!("| {a} | {b} |\n| - | - |\n| {c} | {d} |")),
        1 => "[a-z]{1,6}".prop_map(|c| format!("```rust\n{c}\n```")),
    ]
}

pub fn document() -> impl Strategy<Value = Content> {
    (prop::collection::vec(block(), 1..6), any::<u8>()).prop_map(|(bs, ext)| {
        let mut md = bs.join("\n\n");
        if md.contains("[^n]") {
            md.push_str("\n\n[^n]: a note");
        }
        let mut c = with_ext(from_markdown(&md).unwrap().content, ext);
        let len = c.len_scalars();
        if len > 3 {
            c.anchors.push(Anchor::new("c-0", 1, len.min(4)));
            c.anchors.push(Anchor::new("c-1", 2, 2));
        }
        for (n, island) in c.islands.iter_mut().enumerate() {
            if let Some(slot) = island.slots.values_mut().find(|s| s.len_scalars() > 0) {
                slot.anchors.push(Anchor::new(format!("s-{n}"), 0, 1));
            }
        }
        c.normalize();
        c.validate().unwrap();
        c
    })
}

/// On some documents, a reserved `x-` name on each axis: an island holding a
/// slot at the start, a block's kind and a mark.
pub fn with_ext(mut c: Content, seed: u8) -> Content {
    if seed % 5 == 0 {
        let island = Island::new("isl-x", IslandType::Ext("x-chart".into()))
            .with_props(json!({"series": [1, 2], "style": {"tone": "warm"}}))
            .with_slots([("cap".to_string(), quillmark_content::markdown::from_plaintext("caption"))].into());
        let insert = Change::new(vec![Step::Island(IslandStep::Insert { at: 0, island, block: None })]);
        if let Ok(done) = apply(&c, &insert) {
            c = done.content;
        }
    }
    if seed % 4 == 0 {
        let k = seed as usize / 4 % c.blocks.len();
        c.blocks[k].kind = BlockKind::Ext { name: "x-callout".into(), attrs: json!({"tone": "warm"}).as_object().unwrap().clone() };
    }
    let len = c.len_scalars();
    if seed % 3 == 0 && len > 1 {
        c.marks.push(Mark::new(1, len.min(5), MarkKind::Ext { name: "x-glow".into(), attrs: Default::default() }));
    }
    c
}

pub type Op = (u8, usize, usize, u8);

pub fn ops() -> impl Strategy<Value = Vec<Op>> {
    prop::collection::vec((0u8..15, any::<usize>(), any::<usize>(), any::<u8>()), 1..8)
}

pub const INSERTS: [&str; 7] = ["x", "yz", " ", "\n", "q\nr", "\u{2028}", "😀a"];
pub const TAGS: [&str; 5] = ["strong", "emph", "code", "link", "x-glow"];

pub fn kind_of(tag: &str) -> MarkKind {
    match tag {
        "strong" => MarkKind::Strong,
        "emph" => MarkKind::Emph,
        "code" => MarkKind::Code,
        "x-glow" => MarkKind::Ext { name: tag.into(), attrs: Default::default() },
        _ => MarkKind::Link { url: "https://y.io".into() },
    }
}

pub fn chars(c: &Content) -> Vec<char> {
    c.text.chars().collect()
}

/// A splice on one level, deleting up to 3 characters or, on every third
/// `y`, up to 15, its new `\n`s shaped like the block it sits in.
pub fn splice_on(c: &Content, x: usize, y: usize, z: u8, k: usize) -> Splice {
    let cs = chars(c);
    let len = cs.len();
    let at = x % (len + 1);
    let reach = if y / 16 % 3 == 0 { 16 } else { 4 };
    let dn = y % (len - at + 1).min(reach);
    let insert = INSERTS[z as usize % INSERTS.len()];
    let shape = c.blocks[c.block_at(at)].clone();
    let blocks = insert
        .chars()
        .filter(|&ch| ch == '\n')
        .enumerate()
        .map(|(j, _)| Block { id: format!("n{k}x{j}"), ..shape.clone() })
        .collect();
    let mut s = Splice { blocks, ..Splice::new(at, cs[at..at + dn].iter().collect::<String>(), insert) };
    if z % 3 == 0 && !insert.is_empty() {
        s.marks.push(Mark::new(0, insert.chars().count(), MarkKind::Strong));
    }
    s
}

/// The text `c` holds over `[start, end)`, the quote a writer that read it
/// gives, on every other `seed`.
pub fn quote_of(c: &Content, start: usize, end: usize, seed: usize) -> Option<String> {
    (seed / 2 % 2 == 0).then(|| chars(c)[start..end].iter().collect())
}

/// An anchor step to `anchor`: a move of the one `c`'s text holds under its
/// id, else a placement.
pub fn anchor_step(c: &Content, anchor: Anchor, quote: Option<String>) -> Step {
    match c.anchor(&anchor.id) {
        Some(_) => Step::Anchor(AnchorStep::Move { anchor, quote }),
        None => Step::Anchor(AnchorStep::Place { anchor, quote }),
    }
}

pub fn slots_of(c: &Content) -> Vec<(&Island, String)> {
    c.islands.iter().flat_map(|i| i.slot_order().into_iter().map(move |s| (i, s))).collect()
}

pub fn step_for(c: &Content, (op, x, y, z): Op, k: usize) -> Option<Step> {
    let len = c.len_scalars();
    Some(match op {
        0 | 1 => Step::Splice(splice_on(c, x, y, z, k)),
        2 => {
            let (a, b) = (x % (len + 1), y % (len + 1));
            let (start, end) = (a.min(b), a.max(b));
            let tag = TAGS[z as usize % TAGS.len()];
            let quote = quote_of(c, start, end, y);
            if z % 2 == 0 {
                Step::Mark(MarkStep::Add { mark: Mark::new(start, end, kind_of(tag)), quote })
            } else {
                Step::Mark(MarkStep::Remove { start, end, tag: tag.into(), quote })
            }
        }
        3 => {
            let (a, b) = (x % (len + 1), y % (len + 1));
            match z % 3 {
                0 if !c.anchors.is_empty() => {
                    Step::Anchor(AnchorStep::Remove { id: c.anchors[x % c.anchors.len()].id.clone() })
                }
                _ => anchor_step(c, Anchor::new(format!("c-{}", z % 4), a.min(b), a.max(b)), quote_of(c, a.min(b), a.max(b), x)),
            }
        }
        4 => {
            let b = &c.blocks[x % c.blocks.len()];
            let kind = match z % 5 {
                0 => ("heading", Some(json!({"level": 1 + (y % 6)}))),
                1 => ("para", None),
                2 => ("x-callout", Some(json!({"tone": "cool"}))),
                3 => ("x-callout", None),
                _ => ("code", None),
            };
            Step::SetBlock(SetBlock {
                id: b.id.clone(),
                kind: Some(kind.0.into()),
                attrs: kind.1.and_then(|v| v.as_object().cloned()),
                path: None,
            })
        }
        5 => {
            let b = &c.blocks[x % c.blocks.len()];
            let path = match z % 2 {
                0 => Vec::new(),
                _ => c.blocks[y % c.blocks.len()].path.clone(),
            };
            Step::SetBlock(SetBlock { id: b.id.clone(), path: Some(path), ..SetBlock::default() })
        }
        6 => {
            let with = slots_of(c);
            let (island, slot) = with.get(x % with.len().max(1))?;
            let inner = &island.slots[slot];
            let mut s = splice_on(inner, y, x / 7, z, k);
            s.blocks.iter_mut().for_each(|b| b.path.clear());
            Step::Nested { island: island.id.clone(), slot: slot.clone(), change: Change::new(vec![Step::Splice(s)]) }
        }
        7 => {
            let at = x % (len + 1);
            let island = match z % 2 {
                0 => Island::new(format!("isl-n{k}"), IslandType::Math).with_props(json!({"tex": "y"})),
                _ => {
                    let mut i = Island::new(format!("isl-n{k}"), IslandType::Footnote);
                    i.slots.insert("note".into(), quillmark_content::markdown::from_plaintext("n"));
                    i
                }
            };
            let block = (y % 2 == 0).then(|| c.blocks[c.block_at(at)].id.clone());
            Step::Island(IslandStep::Insert { at, island, block })
        }
        8 => {
            let tables: Vec<&Island> = c.islands.iter().filter(|i| i.island_type == IslandType::Table).collect();
            let island = tables.get(x % tables.len().max(1))?;
            let mut grid = Table::read(&island.props)?;
            let mut slots = std::collections::BTreeMap::new();
            match z % 3 {
                0 if grid.rows.len() > 1 => {
                    grid.rows.remove(y % grid.rows.len());
                }
                1 if grid.columns.len() > 1 => {
                    let col = y % grid.columns.len();
                    grid.columns.remove(col);
                    grid.rows.iter_mut().for_each(|r| {
                        r.cells.remove(col);
                    });
                }
                2 => {
                    let cells: Vec<String> = (0..grid.columns.len()).map(|j| format!("s-n{k}x{j}")).collect();
                    for (j, cell) in cells.iter().enumerate() {
                        let mut inner = quillmark_content::markdown::from_plaintext("new");
                        if j == 0 {
                            inner.anchors.push(Anchor::new(format!("ra-{k}"), 0, 2));
                        }
                        slots.insert(cell.clone(), inner);
                    }
                    grid.rows.insert(y % (grid.rows.len() + 1), Row { id: format!("r-n{k}"), cells });
                }
                _ => return None,
            }
            let mut props = island.props.as_object()?.clone();
            grid.write_into(&mut props);
            Step::Island(IslandStep::Props { id: island.id.clone(), props: props.into(), slots })
        }
        9 => {
            let id = format!("q-n{k}");
            let b = c.blocks[x % c.blocks.len()].clone();
            let mut path = b.path.clone();
            path.insert(0, id.clone());
            return Some(Step::Nested {
                island: String::new(),
                slot: String::new(),
                change: Change::new(vec![
                    Step::Container(ContainerStep::Set(Container::new(id, ContainerKind::Quote))),
                    Step::SetBlock(SetBlock { id: b.id, path: Some(path), ..SetBlock::default() }),
                ]),
            });
        }
        10 | 11 => {
            let with = slots_of(c);
            let (island, slot) = with.get(x % with.len().max(1))?;
            let inner = &island.slots[slot];
            let n = inner.len_scalars();
            let (a, b) = (y % (n + 1), y / 3 % (n + 1));
            let (start, end) = (a.min(b), a.max(b));
            let quote = quote_of(inner, start, end, z as usize);
            let tag = TAGS[z as usize % TAGS.len()];
            let step = match (op, z % 3) {
                (10, 0) => Step::Mark(MarkStep::Remove { start, end, tag: tag.into(), quote }),
                (10, _) if start < end => Step::Mark(MarkStep::Add { mark: Mark::new(start, end, kind_of(tag)), quote }),
                (11, 0) if !inner.anchors.is_empty() => {
                    Step::Anchor(AnchorStep::Remove { id: inner.anchors[x % inner.anchors.len()].id.clone() })
                }
                (11, _) => anchor_step(inner, Anchor::new(format!("sa-{}", z % 4), start, end), quote),
                _ => return None,
            };
            Step::Nested { island: island.id.clone(), slot: slot.clone(), change: Change::new(vec![step]) }
        }
        12 => {
            let mut note = quillmark_content::markdown::from_plaintext("nb");
            note.anchors.push(Anchor::new(format!("fa-{k}"), 0, 1));
            let mut island = Island::new(format!("isl-f{k}"), IslandType::Footnote);
            island.slots.insert("note".into(), note);
            let mut s = splice_on(c, x, y, z, k);
            s.insert.push(ISLAND_SLOT);
            s.islands = vec![island];
            Step::Splice(s)
        }
        13 => {
            let held = c.containers.get(x % c.containers.len().max(1))?;
            let kind = match held.kind {
                ContainerKind::List { ordered, start } => ContainerKind::List { ordered: ordered != (z % 2 == 0), start: start + y as u64 % 3 },
                ContainerKind::Item { .. } => ContainerKind::Item { checked: [None, Some(false), Some(true)][y % 3] },
                ContainerKind::Quote => return None,
            };
            Step::Container(ContainerStep::Set(Container::new(held.id.clone(), kind)))
        }
        _ => return island_move(c, x, y, z),
    })
}

/// An island moved as `diff` moves one: its slot, with the `\n` opening its
/// block where it sits alone in one, deleted and inserted at another block
/// boundary or offset under the same id, the copy first on every other `z`.
pub fn island_move(c: &Content, x: usize, y: usize, z: u8) -> Option<Step> {
    let island = c.islands.get(x % c.islands.len().max(1))?.clone();
    let cs = chars(c);
    let slot = cs.iter().enumerate().filter(|(_, ch)| **ch == ISLAND_SLOT).nth(x % c.islands.len())?.0;
    let k = c.block_at(slot);
    let alone = k > 0 && c.block_texts()[k] == ISLAND_SLOT.to_string();
    let (from, chunk, blocks) = match alone {
        true => (slot - 1, format!("\n{ISLAND_SLOT}"), vec![c.blocks[k].clone()]),
        false => (slot, ISLAND_SLOT.to_string(), Vec::new()),
    };
    let n = chunk.chars().count();
    let to = match alone {
        true => {
            let ends: Vec<usize> = (0..=cs.len()).filter(|&i| (i == cs.len() || cs[i] == '\n') && (i < from || i > from + n)).collect();
            *ends.get(y % ends.len().max(1))?
        }
        false => (0..=cs.len()).filter(|&i| i < from || i > from + n).nth(y % (cs.len() - n).max(1))?,
    };
    let insert = |at| Step::Splice(Splice { blocks: blocks.clone(), islands: vec![island.clone()], ..Splice::new(at, "", chunk.clone()) });
    let delete = |at| Step::Splice(Splice::new(at, chunk.clone(), ""));
    let steps = match (z % 2 == 0, to < from) {
        (true, true) => vec![insert(to), delete(from + n)],
        (true, false) => vec![insert(to), delete(from)],
        (false, true) => vec![delete(from), insert(to)],
        (false, false) => vec![delete(from), insert(to - n)],
    };
    Some(Step::Nested { island: String::new(), slot: String::new(), change: Change::new(steps) })
}

/// A change built against `d`, each step kept only where the value it reads
/// takes it.
pub fn change_for(d: &Content, ops: &[Op]) -> Change {
    change_from(d, ops, 0)
}

/// [`change_for`], the ids it mints numbered from `k0`.
pub fn change_from(d: &Content, ops: &[Op], k0: usize) -> Change {
    let mut sim = d.clone();
    let mut steps = Vec::new();
    for (k, op) in ops.iter().enumerate() {
        let Some(step) = step_for(&sim, *op, k0 + k) else { continue };
        let group = match step {
            Step::Nested { island, change, .. } if island.is_empty() => change.steps,
            step => vec![step],
        };
        let mut trial = steps.clone();
        trial.extend(group.iter().cloned());
        if let Ok(done) = apply(d, &Change::new(trial.clone())) {
            steps = trial;
            sim = done.content;
        }
    }
    Change::new(steps)
}

/// A change on `d` that touches only the text in `[lo, hi]`, the window
/// following what its own splices do, deleting no `\n`, so it joins no
/// block another change edits, its anchors named `{side}-*`.
pub fn windowed(d: &Content, ops: &[Op], (lo, mut hi): (usize, usize), side: &str) -> Change {
    let mut sim = d.clone();
    let mut steps = Vec::new();
    for (k, &(op, x, y, z)) in ops.iter().enumerate() {
        let (a, b) = (lo + x % (hi - lo + 1), lo + y % (hi - lo + 1));
        let (start, end) = (a.min(b), a.max(b));
        let step = match op % 3 {
            0 => {
                let mut s = splice_on(&sim, start, 0, z, k);
                let cs = chars(&sim);
                let dn = cs[start..end].iter().take(3).take_while(|&&ch| ch != '\n').count();
                s.delete = cs[start..start + dn].iter().collect();
                Step::Splice(s)
            }
            1 if start < end => Step::Mark(MarkStep::Add { mark: Mark::new(start, end, kind_of(TAGS[z as usize % TAGS.len()])), quote: quote_of(&sim, start, end, y) }),
            _ => anchor_step(&sim, Anchor::new(format!("{side}-{}", z % 3), start, end), quote_of(&sim, start, end, x)),
        };
        let mut trial = steps.clone();
        trial.push(step.clone());
        if let Ok(done) = apply(d, &Change::new(trial.clone())) {
            if let Step::Splice(s) = &step {
                hi = hi + s.insert.chars().count() - s.delete.chars().count();
            }
            steps = trial;
            sim = done.content;
        }
    }
    Change::new(steps)
}
